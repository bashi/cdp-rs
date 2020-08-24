use rustyline::error::ReadlineError;
use rustyline::Editor;

use super::Error;
use crate::endpoints::Endpoints;

enum Command {
    Version,
    List,
    NewTab(String),
}

fn parse_command_line(line: &str) -> Option<Command> {
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

    None
}

async fn exec_endpoint_command(endpoints: Endpoints, line: String) -> Result<(), Error> {
    let command = match parse_command_line(line.as_str()) {
        Some(command) => command,
        None => {
            println!("Unknown command: {}", line);
            return Ok(());
        }
    };

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
    }

    Ok(())
}

pub(crate) async fn run(endpoints: Endpoints) -> Result<(), Error> {
    let mut rl = Editor::<()>::new();
    loop {
        let readline = rl.readline("cdp> ");
        match readline {
            Ok(line) => {
                rl.add_history_entry(line.as_str());
                exec_endpoint_command(endpoints.clone(), line).await?;
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
