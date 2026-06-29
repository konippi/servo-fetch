//! W3C `WebDriver` server subcommand.

use crate::cli::WebdriverArgs;

pub(crate) fn run(args: &WebdriverArgs) -> anyhow::Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(crate::webdriver::run(&args.host, args.port))
}
