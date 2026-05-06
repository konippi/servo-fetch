const { chromium } = require("playwright");

const TIMEOUT_MS = 30_000;

const COMMON_ARGS = [
    "--disable-gpu",
    "--disable-extensions",
    "--disable-background-timer-throttling",
    "--disable-backgrounding-occluded-windows",
    "--disable-renderer-backgrounding",
    "--no-first-run",
    "--disable-background-networking",
];

// Linux-only: --no-sandbox (SIGTRAPs on macOS with raw chromium binaries),
// --disable-dev-shm-usage (Docker /dev/shm workaround, irrelevant on macOS).
const LINUX_ARGS = process.platform === "linux"
    ? ["--no-sandbox", "--disable-dev-shm-usage"]
    : [];

const OPTIMIZED_ARGS = [...COMMON_ARGS, ...LINUX_ARGS];

async function fetchText(browser, url) {
    const page = await browser.newPage();
    try {
        await page.goto(url, { waitUntil: "load", timeout: TIMEOUT_MS });
        return await page.evaluate(() => document.body.innerText);
    } catch (e) {
        process.stderr.write(`playwright-runner: ${url}: ${e.stack ?? e.message}\n`);
        return "";
    } finally {
        await page.close();
    }
}

async function main() {
    const urls = process.argv.slice(2);
    if (!urls.length) {
        process.stderr.write("usage: playwright-runner.js <url>...\n");
        process.exit(2);
    }
    const browser = await chromium.launch({ headless: true, args: OPTIMIZED_ARGS });
    try {
        const results = await Promise.all(urls.map((url) => fetchText(browser, url)));
        process.stdout.write(results.join("\n\f\n") + "\n");
    } finally {
        await browser.close();
    }
}

main().catch((e) => {
    process.stderr.write(`playwright-runner: ${e.stack ?? e.message}\n`);
    process.exit(1);
});
