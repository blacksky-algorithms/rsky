use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(about = "Convert a multi-tenant space store into per-account actor stores")]
struct Args {
    /// The deployed multi-tenant sqlite file, opened read-only.
    #[arg(long)]
    from: PathBuf,
    /// Directory the per-account `store.sqlite` files are written under.
    #[arg(long)]
    into: PathBuf,
}

fn main() -> Result<(), rsky_space_host::HostError> {
    let args = Args::parse();
    let totals = rsky_space_host::convert::convert(&args.from, &args.into)?;
    println!(
        "converted {} accounts, {} repos, {} records, {} ops",
        totals.accounts, totals.repos, totals.records, totals.ops
    );
    Ok(())
}
