use async_net::TcpStream;
use rand::Rng;
use smol::io;
use smol::prelude::*;
use url::Url;

use crate::endpoints::read_raw_header;
use crate::Error;

#[derive(Debug, Copy, Clone)]
pub(crate) enum Opcode {
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
pub(crate) struct FrameHeader {
    pub(crate) fin: bool,
    pub(crate) opcode: Opcode,
    pub(crate) mask: bool,
    pub(crate) payload_len: usize,
    pub(crate) masking_key: Option<[u8; 4]>,
}

#[derive(Debug)]
pub(crate) struct Frame {
    pub(crate) header: FrameHeader,
    pub(crate) payload: Vec<u8>,
}

pub(crate) struct Sender {
    stream: TcpStream,
}

impl Sender {
    fn new(stream: TcpStream) -> Self {
        Sender { stream }
    }

    pub(crate) fn send_text_frame(&self, text: String) -> impl Future<Output = Result<(), Error>> {
        send_text_frame(self.stream.clone(), text)
    }
}

pub(crate) struct Receiver {
    reader: io::BufReader<TcpStream>,
}

impl Receiver {
    fn new(stream: TcpStream) -> Self {
        let reader = io::BufReader::new(stream);
        Receiver { reader }
    }

    pub(crate) async fn receive_frame(&mut self) -> Result<Frame, Error> {
        receive_frame(&mut self.reader).await
    }
}

pub(crate) async fn connect(url: Url) -> Result<(Sender, Receiver), Error> {
    let stream = connect_stream(url).await?;
    let sender = Sender::new(stream.clone());
    let receiver = Receiver::new(stream);
    Ok((sender, receiver))
}

async fn connect_stream(url: Url) -> Result<TcpStream, Error> {
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
    read_raw_header(&mut reader, &mut buf).await?;

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

    Ok(stream)
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

async fn receive_frame(reader: &mut io::BufReader<TcpStream>) -> Result<Frame, Error> {
    let header = read_header(reader).await?;
    if header.mask {
        return Err(format!("Frame should not be masked").into());
    }

    let mut payload = vec![0; header.payload_len];
    reader.read_exact(&mut payload).await?;

    Ok(Frame { header, payload })
}

async fn send_text_frame(mut stream: TcpStream, text: String) -> Result<(), Error> {
    let masking_key = rand::thread_rng().gen::<[u8; 4]>();

    let mut payload = vec![0; text.len()];
    let data = text.as_bytes();
    for i in 0..text.len() {
        payload[i] = data[i] ^ masking_key[i % 4];
    }

    let header = FrameHeader {
        fin: true,
        opcode: Opcode::TextFrame,
        mask: true,
        payload_len: payload.len(),
        masking_key: Some(masking_key),
    };

    write_header(&mut stream, &header).await?;
    stream.write_all(&payload).await?;

    Ok(())
}

async fn write_header(stream: &mut TcpStream, header: &FrameHeader) -> Result<(), Error> {
    let mut buf = [0; 10];
    buf[0] = ((header.fin as u8) << 7) | header.opcode as u8;
    buf[1] = (header.mask as u8) << 7;

    let len = header.payload_len;
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
    if let Some(masking_key) = header.masking_key.as_ref() {
        stream.write_all(masking_key).await?;
    }

    Ok(())
}

async fn read_header(reader: &mut io::BufReader<TcpStream>) -> Result<FrameHeader, Error> {
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

    Ok(FrameHeader {
        fin,
        opcode,
        mask,
        payload_len,
        masking_key,
    })
}
