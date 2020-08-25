use async_channel::{Receiver, Sender};

use rustyline::error::ReadlineError;
use rustyline::Editor;

use super::Error;
use crate::endpoints::Endpoints;
use crate::websocket_target::WebSocketTarget;

enum Command {
    Version,
    List,
    NewTab(String),
    ConnectWebSocketTarget(String),
    MethodCall(MethodCall),
    Unknown(String),
}

#[derive(Debug)]
pub(crate) struct MethodCall {
    domain: String,
    name: String,
    params: serde_json::Value,
}

impl MethodCall {
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

fn parse_command_line(line: &str) -> Option<Command> {
    if line.len() == 0 {
        return None;
    }

    if line == "version" {
        return Some(Command::Version);
    }

    if line == "list" {
        return Some(Command::List);
    }

    const NEW_TAB_COMMAND: &str = "newtab ";
    if line.starts_with(NEW_TAB_COMMAND) {
        let url = line[NEW_TAB_COMMAND.len()..].to_string();
        return Some(Command::NewTab(url));
    }

    const CONNECT_WEBSOCKET_TARGET_COMMAND: &str = "connect ";
    if line.starts_with(CONNECT_WEBSOCKET_TARGET_COMMAND) {
        let url = line[CONNECT_WEBSOCKET_TARGET_COMMAND.len()..].to_string();
        return Some(Command::ConnectWebSocketTarget(url));
    }

    if let Some(msg) = parse_method_call(line) {
        return Some(Command::MethodCall(msg));
    }

    Some(Command::Unknown(line.to_owned()))
}

pub(crate) async fn execute_command(
    endpoints: Endpoints,
    lines: Receiver<String>,
) -> Result<(), Error> {
    endpoints.version().await?;

    // Tentative
    const NEWTAB_URL: &'static str = "chrome://newtab/";
    let targets = endpoints.clone().target_list().await?;
    let newtab = targets.into_iter().find(|t| t.url == NEWTAB_URL);
    let target_url = match newtab {
        Some(newtab) => newtab.websocket_debugger_url,
        None => {
            let newtab = endpoints.clone().open_new_tab(NEWTAB_URL).await?;
            newtab.websocket_debugger_url
        }
    };
    let target_url = url::Url::parse(&target_url)?;
    let mut target = WebSocketTarget::connect(target_url).await?;
    smol::Task::spawn(target.receive_frames()).detach();

    while let Ok(line) = lines.recv().await {
        let command = match parse_command_line(line.as_str()) {
            Some(command) => command,
            None => {
                continue;
            }
        };

        let endpoints = endpoints.clone();
        match command {
            Command::Version => {
                let res = endpoints.version().await?;
                println!("{:#?}", res);
            }
            Command::List => {
                let res = endpoints.target_list().await?;
                println!("{:#?}", res);
            }
            Command::NewTab(url) => {
                let res = endpoints.open_new_tab(url).await?;
                println!("{:#?}", res);
            }
            Command::ConnectWebSocketTarget(url) => {
                let url = url::Url::parse(url.as_str())?;
                WebSocketTarget::connect(url).await?;
            }
            Command::MethodCall(method) => {
                println!("{:?}", method);
                target.call_method(&method).await?;
            }
            Command::Unknown(line) => {
                println!("Unknown command: {}", line);
            }
        }
    }

    Ok(())
}

pub(crate) async fn run_repl(sender: Sender<String>) -> Result<(), Error> {
    let mut rl = Editor::<()>::new();
    loop {
        let readline = rl.readline("cdp> ");
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str());
                sender.send(line).await?;
            }
            Err(ReadlineError::Interrupted) => {
                // println!("Ctrl-C");
                break;
            }
            Err(ReadlineError::Eof) => {
                // println!("Ctrl-D");
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }
    Ok(())
}
