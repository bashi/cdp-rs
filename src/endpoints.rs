use async_net::TcpStream;
use serde::Deserialize;
use smol::{io, prelude::*};

use crate::Error;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetItem {
    pub description: String,
    pub devtools_frontend_url: String,
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub target_type: String,
    pub url: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    pub websocket_debugger_url: String,
}

#[derive(Debug, Deserialize)]
pub struct BrowserVersionMetadata {
    #[serde(rename = "Browser")]
    pub browser: String,
    #[serde(rename = "Protocol-Version")]
    pub protocol_version: String,
    #[serde(rename = "User-Agent")]
    pub user_agent: String,
    #[serde(rename = "V8-Version")]
    pub v8_version: String,
    #[serde(rename = "WebKit-Version")]
    pub webkit_version: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    pub websocket_debugger_url: String,
}

const MAX_HEADERS: usize = 64;
const MAX_HEADER_LEN: usize = 8192;

pub(crate) async fn read_header(
    reader: &mut io::BufReader<&TcpStream>,
    buf: &mut Vec<u8>,
) -> Result<(), Error> {
    loop {
        let n = reader.read_until(b'\n', buf).await?;
        if n == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof).into());
        }

        let len = buf.len();
        if len > 4 && &buf[len - 4..] == b"\r\n\r\n" {
            break;
        }

        if len > MAX_HEADER_LEN {
            return Err(format!("Header too large").into());
        }
    }
    Ok(())
}

async fn endpoint_response(stream: &TcpStream) -> Result<Vec<u8>, Error> {
    let mut reader = io::BufReader::new(stream);

    // Read http header
    let mut buf = Vec::new();
    read_header(&mut reader, &mut buf).await?;

    // Parse
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut response = httparse::Response::new(&mut headers);
    match response.parse(&buf)? {
        httparse::Status::Partial => {
            return Err(format!("Invalid header").into());
        }
        httparse::Status::Complete(_) => (),
    }

    if response.code.unwrap_or(0) != 200 {
        return Err(format!("Response != 200").into());
    }

    // Headers
    let mut content_length = 0;
    let mut content_type = None;
    for header in response.headers {
        match header.name {
            "Content-Length" => {
                let value = std::str::from_utf8(header.value)?;
                content_length = value.parse::<usize>()?;
            }
            "Content-Type" => {
                let value = std::str::from_utf8(header.value)?;
                content_type = Some(value);
            }
            _ => (),
        }
    }

    if content_length == 0 {
        return Err(format!("No content").into());
    }
    if content_type.unwrap_or("") != "application/json; charset=UTF-8" {
        return Err(format!("Content is not json").into());
    }

    let mut buf = vec![0; content_length];
    reader.read_exact(&mut buf).await?;

    Ok(buf)
}

async fn send_request(stream: &mut TcpStream, host: &str, path: &str) -> Result<Vec<u8>, Error> {
    let path = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, host
    );
    stream.write_all(path.as_bytes()).await?;
    let content = endpoint_response(&stream).await?;
    Ok(content)
}

#[derive(Clone)]
pub struct Endpoints {
    host: String,
    port: u16,
    stream: TcpStream,
}

impl Endpoints {
    pub(crate) async fn new(host: impl Into<String>, port: u16) -> Result<Self, Error> {
        let host = host.into();
        let stream = TcpStream::connect(&format!("{}:{}", host, port)).await?;
        Ok(Endpoints { host, port, stream })
    }

    pub fn version(&self) -> impl Future<Output = Result<BrowserVersionMetadata, Error>> {
        let mut stream = self.stream.clone();
        let host = self.host.clone();
        async move {
            let content = send_request(&mut stream, &host, "/json/version").await?;
            let version: BrowserVersionMetadata = serde_json::from_slice(&content)?;
            Ok(version)
        }
    }

    pub fn target_list(&self) -> impl Future<Output = Result<Vec<TargetItem>, Error>> {
        let mut stream = self.stream.clone();
        let host = self.host.clone();
        async move {
            let content = send_request(&mut stream, &host, "/json/list").await?;
            let targets: Vec<TargetItem> = serde_json::from_slice(&content)?;
            Ok(targets)
        }
    }

    pub fn open_new_tab(
        &self,
        url: impl AsRef<str>,
    ) -> impl Future<Output = Result<TargetItem, Error>> {
        let mut stream = self.stream.clone();
        let path = format!("/json/new?{}", url.as_ref());
        let host = self.host.clone();
        async move {
            let content = send_request(&mut stream, &host, &path).await?;
            let target: TargetItem = serde_json::from_slice(&content)?;
            Ok(target)
        }
    }

    pub fn activate(&self, target_id: impl AsRef<str>) -> impl Future<Output = Result<(), Error>> {
        let mut stream = self.stream.clone();
        let path = format!("/json/activate/{}", target_id.as_ref());
        let host = self.host.clone();
        async move {
            let _content = send_request(&mut stream, &host, &path).await?;
            Ok(())
        }
    }

    pub fn close(&self, target_id: impl AsRef<str>) -> impl Future<Output = Result<(), Error>> {
        let mut stream = self.stream.clone();
        let path = format!("/json/activate/{}", target_id.as_ref());
        let host = self.host.clone();
        async move {
            let _content = send_request(&mut stream, &host, &path).await?;
            Ok(())
        }
    }
}
