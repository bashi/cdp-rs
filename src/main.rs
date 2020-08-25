use async_channel::bounded;
use smol::Task;
use structopt::StructOpt;

mod cli;
pub mod endpoints;
mod websocket_target;

use endpoints::Endpoints;

#[derive(Debug, StructOpt)]
#[structopt(
    name = "cdp-cli",
    about = "A commandline tool for Chrome DevTools Protocol"
)]
struct Opt {
    #[structopt(long, default_value = "localhost")]
    host: String,
    #[structopt(long, default_value = "9222")]
    port: u16,
}

type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

fn main() -> Result<(), Error> {
    let opt = Opt::from_args();

    let endpoints = Endpoints::new(&opt.host, opt.port);

    smol::run(async move {
        let (sender, receiver) = bounded(100);
        Task::spawn(cli::execute_command(endpoints, receiver)).detach();
        Task::spawn(cli::run_repl(sender)).await?;
        Ok(())
    })
}
