use clap::Parser;

#[derive(Parser)]
pub struct Args {
    #[arg(short, long, default_value = "/dev/sda")]
    pub device: String,

    #[arg(short, long, default_value = "1GiB")]
    pub efi: String,

    #[arg(short, long, default_value = "8GiB")]
    pub swap: String,
}