use serde::Deserialize;

use async_net::TcpStream;
use smol::{io, prelude::*};

type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

const MAX_HEADERS: usize = 64;
const MAX_HEADER_LEN: usize = 8192;

async fn endpoint_response(stream: &TcpStream) -> Result<Vec<u8>, Error> {
    let mut reader = io::BufReader::new(stream);

    // Read http header
    let mut buf = Vec::new();
    loop {
        let n = reader.read_until(b'\n', &mut buf).await?;
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

#[derive(Clone, Debug, Deserialize)]
pub struct TargetId(String);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetItem {
    pub description: String,
    pub devtools_frontend_url: String,
    pub id: TargetId,
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

#[derive(Clone)]
pub struct Endpoints {
    host: String,
    port: u16,
}

impl Endpoints {
    pub(crate) fn new(host: impl Into<String>, port: u16) -> Self {
        let host = host.into();
        Endpoints { host, port }
    }

    async fn send_request(self, path: &str) -> Result<Vec<u8>, Error> {
        let mut stream = TcpStream::connect(&format!("{}:{}", self.host, self.port)).await?;
        let path = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
            path, self.host
        );
        stream.write_all(path.as_bytes()).await?;
        let content = endpoint_response(&stream).await?;
        Ok(content)
    }

    pub async fn version(self) -> Result<BrowserVersionMetadata, Error> {
        let content = self.send_request("/json/version").await?;
        let version: BrowserVersionMetadata = serde_json::from_slice(&content)?;
        Ok(version)
    }

    pub async fn target_list(self) -> Result<Vec<TargetItem>, Error> {
        let content = self.send_request("/json/list").await?;
        let targets: Vec<TargetItem> = serde_json::from_slice(&content)?;
        Ok(targets)
    }

    pub async fn open_new_tab(self, url: impl AsRef<str>) -> Result<TargetItem, Error> {
        let path = format!("/json/new?{}", url.as_ref());
        let content = self.send_request(&path).await?;
        let target: TargetItem = serde_json::from_slice(&content)?;
        Ok(target)
    }

    pub async fn activate(self, target_id: TargetId) -> Result<(), Error> {
        let path = format!("/json/activate/{}", target_id.0);
        let _content = self.send_request(&path).await?;
        Ok(())
    }

    pub async fn close(self, target_id: TargetId) -> Result<(), Error> {
        let path = format!("/json/close/{}", target_id.0);
        let _content = self.send_request(&path).await?;
        Ok(())
    }
}
