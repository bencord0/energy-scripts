use clap::Parser;
use eyre::Error;
use power::OctopusClient;
use std::env;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    account_id: String,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args = Args::parse();
    let api_key = env::var("OCTOPUS_API_KEY")
        .map_err(|_| eyre::eyre!("OCTOPUS_API_KEY environment variable not set"))?;

    let client = OctopusClient::new().api_key(api_key);
    let account = client.get_account(&args.account_id).await?;

    println!("{}", serde_json::to_string_pretty(&account)?);

    Ok(())
}
