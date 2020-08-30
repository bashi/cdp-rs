use async_net::TcpStream;
use rand::Rng;
use smol::io;
use smol::prelude::*;
use url::Url;

use super::Error;

use crate::cli::MethodCall;
use crate::endpoints::read_header;

pub(crate) struct WebSocketTarget {
    pub(crate) stream: TcpStream,
    method_id: usize,
}

impl WebSocketTarget {
    pub(crate) async fn connect(url: Url) -> Result<Self, Error> {
        connect(url).await
    }

    pub(crate) fn call_method(
        &mut self,
        method: &MethodCall,
    ) -> impl Future<Output = Result<(), Error>> {
        let msg = method.serialize(self.method_id);
        self.method_id += 1;
        send_text_frame(self.stream.clone(), msg)
    }

    // Tentative
    pub(crate) fn receive_frames(&self) -> impl Future<Output = Result<(), Error>> {
        receive_frames(self.stream.clone())
    }
}

async fn connect(url: Url) -> Result<WebSocketTarget, Error> {
    let host = match url.host_str() {
        Some(host) => host,
        None => return Err("No host".into()),
    };
    let port = match url.port() {
        Some(port) => port,
        None => 9222,
    };
    let path = url.path();
    let origin = format!("http://{}", host);
    let random_value = rand::thread_rng().gen::<[u8; 16]>();
    let key = base64::encode(random_value);

    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nOrigin: {}\r\nSec-WebSocket-Key: {}\r\nSec-WebSocket-Version: 13\r\n\r\n",
        path, host, origin, key
    );

    let mut stream = TcpStream::connect((host, port)).await?;
    stream.write_all(request.as_bytes()).await?;

    // Read header
    let mut buf = Vec::new();
    let mut reader = io::BufReader::new(&stream);
    read_header(&mut reader, &mut buf).await?;

    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut response = httparse::Response::new(&mut headers);
    match response.parse(&buf)? {
        httparse::Status::Partial => {
            return Err(format!("Invalid header").into());
        }
        httparse::Status::Complete(_) => (),
    }

    if response.code.unwrap_or(0) != 101 {
        return Err(format!("Response != 101").into());
    }

    // Verify `Sec-WebSocket-Accept`
    for header in response.headers {
        match header.name {
            "Sec-WebSocket-Accept" => {
                check_sec_websocket_accept(&key, header.value)?;
            }
            _ => (),
        }
    }

    let method_id = 0;
    let target = WebSocketTarget { stream, method_id };
    Ok(target)
}

fn check_sec_websocket_accept(key: &str, accept_value: &[u8]) -> Result<(), Error> {
    use sha1::{Digest, Sha1};
    const ACCEPT_SUFFIX: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let accept = format!("{}{}", key, ACCEPT_SUFFIX);
    let mut hasher = Sha1::new();
    hasher.update(accept.as_bytes());
    let hashed = hasher.finalize();
    let encoded = base64::encode(hashed.as_slice());
    if accept_value == encoded.as_bytes() {
        Ok(())
    } else {
        println!("{:?} != {:?}", accept_value, hashed.as_slice());
        Err(format!("Invalid Sec-WebSocket-Accept: {:?}", accept_value).into())
    }
}

async fn receive_frames(stream: TcpStream) -> Result<(), Error> {
    let mut reader = io::BufReader::new(stream);

    // TODO: Figure out how to terminate the loop.
    loop {
        // Read a single frame
        let (frame, payload) = receive_single_frame(&mut reader).await?;
        assert!(frame.fin, "Fragmented frames aren't supported.");

        let value: serde_json::Value = serde_json::from_slice(&payload)?;
        let res = serde_json::to_string_pretty(&value)?;
        println!("{}", res);
    }
}

async fn receive_single_frame(
    reader: &mut io::BufReader<TcpStream>,
) -> Result<(WebSocketFrame, Vec<u8>), Error> {
    let frame = read_single_frame(reader).await?;
    if frame.mask {
        return Err(format!("Frame should not be masked").into());
    }

    let mut payload = vec![0; frame.payload_len];
    reader.read_exact(&mut payload).await?;

    Ok((frame, payload))
}

async fn send_text_frame(mut stream: TcpStream, text: String) -> Result<(), Error> {
    let masking_key = rand::thread_rng().gen::<[u8; 4]>();

    let mut payload = vec![0; text.len()];
    let data = text.as_bytes();
    for i in 0..text.len() {
        payload[i] = data[i] ^ masking_key[i % 4];
    }

    let frame = WebSocketFrame {
        fin: true,
        opcode: Opcode::TextFrame,
        mask: true,
        payload_len: payload.len(),
        masking_key: Some(masking_key),
    };

    write_single_frame(&mut stream, &frame).await?;
    stream.write_all(&payload).await?;

    Ok(())
}

#[derive(Debug, Copy, Clone)]
enum Opcode {
    ContinuationFrame = 0x0,
    TextFrame = 0x1,
    BinaryFrame = 0x2,
    Close = 0x8,
    Ping = 0x9,
    Pong = 0xa,
}

impl Opcode {
    fn from_u8(value: u8) -> Result<Opcode, Error> {
        match value {
            0x0 => Ok(Opcode::ContinuationFrame),
            0x1 => Ok(Opcode::TextFrame),
            0x2 => Ok(Opcode::BinaryFrame),
            0x8 => Ok(Opcode::Close),
            0x9 => Ok(Opcode::Ping),
            0xa => Ok(Opcode::Pong),
            _ => Err(format!("Invalid opcode: {}", value).into()),
        }
    }
}

#[derive(Debug)]
struct WebSocketFrame {
    fin: bool,
    opcode: Opcode,
    mask: bool,
    payload_len: usize,
    masking_key: Option<[u8; 4]>,
}

async fn write_single_frame(stream: &mut TcpStream, frame: &WebSocketFrame) -> Result<(), Error> {
    let mut buf = [0; 10];
    buf[0] = ((frame.fin as u8) << 7) | frame.opcode as u8;
    buf[1] = (frame.mask as u8) << 7;

    let len = frame.payload_len;
    let size = if len <= 125 {
        buf[1] |= len as u8;
        2
    } else if len <= 65535 {
        buf[1] |= 126;
        buf[2] = (len & 0xff) as u8;
        buf[3] = ((len >> 8) & 0xff) as u8;
        4
    } else {
        buf[1] |= 127;
        buf[2] = ((len >> 0) & 0xff) as u8;
        buf[3] = ((len >> 8) & 0xff) as u8;
        buf[4] = ((len >> 16) & 0xff) as u8;
        buf[5] = ((len >> 24) & 0xff) as u8;
        buf[6] = ((len >> 32) & 0xff) as u8;
        buf[7] = ((len >> 40) & 0xff) as u8;
        buf[8] = ((len >> 48) & 0xff) as u8;
        buf[9] = ((len >> 56) & 0xff) as u8;
        10
    };

    stream.write_all(&buf[..size]).await?;
    if let Some(masking_key) = frame.masking_key.as_ref() {
        stream.write_all(masking_key).await?;
    }

    Ok(())
}

async fn read_single_frame(reader: &mut io::BufReader<TcpStream>) -> Result<WebSocketFrame, Error> {
    let mut first_two = [0; 2];
    reader.read_exact(&mut first_two).await?;
    let fin = first_two[0] & 0x80 == 0x80;
    let opcode = Opcode::from_u8(first_two[0] & 0x0f)?;
    let mask = first_two[1] & 0x80 == 0x80;
    let payload_len = first_two[1] & 0x7f;

    // Deserialize payload length
    let payload_len = if payload_len <= 125 {
        payload_len as usize
    } else if payload_len == 126 {
        let mut buf = [0; 2];
        reader.read_exact(&mut buf).await?;
        ((buf[0] as usize) << 8) | (buf[1] as usize)
    } else {
        // payload_len == 127
        let mut buf = [0; 8];
        reader.read_exact(&mut buf).await?;
        ((buf[0] as usize) << 56)
            | ((buf[1] as usize) << 48)
            | ((buf[2] as usize) << 40)
            | ((buf[3] as usize) << 32)
            | ((buf[4] as usize) << 24)
            | ((buf[5] as usize) << 16)
            | ((buf[6] as usize) << 8)
            | ((buf[7] as usize) << 0)
    };

    let mut masking_key = None;
    if mask {
        let mut buf = [0; 4];
        reader.read_exact(&mut buf).await?;
        masking_key = Some(buf);
    }

    Ok(WebSocketFrame {
        fin,
        opcode,
        mask,
        payload_len,
        masking_key,
    })
}
