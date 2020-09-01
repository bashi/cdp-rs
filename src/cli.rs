use rustyline::error::ReadlineError;
use rustyline::Editor;

use crate::endpoints::Endpoints;
use crate::websocket_target::{MethodCall, WebSocketTarget};
use crate::{Error, Opt};

pub(crate) async fn run_repl(opt: Opt) -> Result<(), Error> {
    let endpoints = Endpoints::new(&opt.host, opt.port).await?;

    // Tentative: Create a new tab if not exists, then set it as the initial target.
    const NEWTAB_URL: &'static str = "chrome://newtab/";
    let targets = endpoints.target_list().await?;
    let newtab = targets.into_iter().find(|t| t.url == NEWTAB_URL);
    let target_url = match newtab {
        Some(newtab) => newtab.websocket_debugger_url,
        None => {
            let newtab = endpoints.open_new_tab(NEWTAB_URL).await?;
            newtab.websocket_debugger_url
        }
    };
    let target_url = url::Url::parse(&target_url)?;
    let mut target = WebSocketTarget::connect(target_url).await?;

    let mut rl = Editor::<()>::new();
    loop {
        let readline = rl.readline("cdp> ");
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str());
                match parse_command_line(&line) {
                    Some(command) => execute_command(command, &endpoints, &mut target).await?,
                    None => (),
                }
            }
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
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

enum Command {
    Version,
    List,
    NewTab(String),
    ConnectTarget(String),
    ActivateTarget(String),
    CloseTarget(String),
    MethodCall(MethodCall),
    Unknown(String),
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

    const CONNECT_TARGET_COMMAND: &str = "connect ";
    if line.starts_with(CONNECT_TARGET_COMMAND) {
        let url = line[CONNECT_TARGET_COMMAND.len()..].to_string();
        return Some(Command::ConnectTarget(url));
    }

    const ACTIVATE_TARGET_COMMAND: &str = "activate ";
    if line.starts_with(ACTIVATE_TARGET_COMMAND) {
        let target_id = line[ACTIVATE_TARGET_COMMAND.len()..].to_string();
        return Some(Command::ActivateTarget(target_id));
    }

    const CLOSE_TARGET_COMMAND: &str = "close ";
    if line.starts_with(CLOSE_TARGET_COMMAND) {
        let target_id = line[CLOSE_TARGET_COMMAND.len()..].to_string();
        return Some(Command::CloseTarget(target_id));
    }

    if let Some(msg) = MethodCall::from_str(line) {
        return Some(Command::MethodCall(msg));
    }

    Some(Command::Unknown(line.to_owned()))
}

async fn execute_command(
    command: Command,
    endpoints: &Endpoints,
    target: &mut WebSocketTarget,
) -> Result<(), Error> {
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
        Command::ConnectTarget(url) => {
            let url = url::Url::parse(url.as_str())?;
            *target = WebSocketTarget::connect(url).await?;
        }
        Command::ActivateTarget(target_id) => {
            endpoints.activate(target_id).await?;
        }
        Command::CloseTarget(target_id) => {
            endpoints.close(target_id).await?;
        }
        Command::MethodCall(method) => {
            println!("{:?}", method);
            target.call_method(&method).await?;
        }
        Command::Unknown(line) => {
            println!("Unknown command: {}", line);
        }
    }
    Ok(())
}
