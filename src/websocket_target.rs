use smol::prelude::*;
use url::Url;

use crate::Error;

use crate::websocket;

#[derive(Debug)]
pub(crate) struct MethodCall {
    domain: String,
    name: String,
    params: serde_json::Value,
}

impl MethodCall {
    pub(crate) fn from_str(s: &str) -> Option<Self> {
        parse_method_call(s)
    }

    pub(crate) fn serialize(&self, id: usize) -> String {
        let msg = serde_json::json!({
            "id": id,
            "method": format!("{}.{}", self.domain, self.name),
            "params": self.params,
        });
        msg.to_string()
    }
}

fn parse_method_call(line: &str) -> Option<MethodCall> {
    // Tentative
    let bytes = line.as_bytes();
    let dot = match bytes.iter().position(|b| *b == b'.') {
        Some(pos) => pos,
        None => return None,
    };

    let remaining = &bytes[dot..];
    let lparen = match remaining.iter().position(|b| *b == b'(') {
        Some(pos) => dot + pos,
        None => return None,
    };

    let remaining = &bytes[lparen..];
    let offset = bytes.len() - lparen;
    let rparen = match remaining.iter().rev().position(|b| *b == b')') {
        Some(pos) => lparen + (offset - pos) - 1,
        None => return None,
    };

    let domain = unsafe { String::from_utf8_unchecked(bytes[..dot].to_vec()) };
    let name = unsafe { String::from_utf8_unchecked(bytes[dot + 1..lparen].to_vec()) };

    let params_bytes = &bytes[lparen + 1..rparen];
    let params = if params_bytes.len() == 0 {
        serde_json::from_str("{}").unwrap()
    } else {
        match serde_json::from_slice(params_bytes) {
            Ok(params) => params,
            Err(_) => return None,
        }
    };

    Some(MethodCall {
        domain,
        name,
        params,
    })
}

pub(crate) struct WebSocketTarget {
    sender: websocket::Sender,
    method_id: usize,
}

impl WebSocketTarget {
    pub(crate) async fn connect(url: Url) -> Result<Self, Error> {
        let method_id = 0;
        let (sender, receiver) = websocket::connect(url).await?;

        // Tentative; remove runtime (smol) dependency
        smol::Task::spawn(receive_frames(receiver)).detach();

        Ok(WebSocketTarget { sender, method_id })
    }

    pub(crate) fn call_method(
        &mut self,
        method: &MethodCall,
    ) -> impl Future<Output = Result<(), Error>> {
        let msg = method.serialize(self.method_id);
        self.method_id += 1;
        self.sender.send_text_frame(msg)
    }
}

async fn receive_frames(mut receiver: websocket::Receiver) -> Result<(), Error> {
    use colored_json::prelude::*;

    // Tentative: Open a file to log events.
    use async_std::fs::File;
    let mut events = File::create("events.log").await?;

    // TODO: Make receiver implement Stream.
    loop {
        let frame = receiver.receive_frame().await?;
        assert!(frame.header.fin, "Fragmented frames aren't supported.");

        let value: serde_json::Value = serde_json::from_slice(&frame.payload)?;
        if let Some(ref _msg_id) = value.get("id") {
            // This is a reply for a method call.
            let res = serde_json::to_string_pretty(&value)?;
            println!("{}", res.to_colored_json_auto()?);
        } else {
            // This is an event coming from DevTools.
            let res = serde_json::to_string_pretty(&value)?;
            let res = res.to_colored_json_auto()?;
            events.write_all(res.as_bytes()).await?;
            events.write_all(b"\n").await?;
            events.sync_all().await?;
        }
    }
}
