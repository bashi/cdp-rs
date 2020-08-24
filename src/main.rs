use smol::Task;
use structopt::StructOpt;

mod cli;
pub mod endpoints;

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
        Task::spawn(cli::run(endpoints)).await?;
        Ok(())
    })
}
