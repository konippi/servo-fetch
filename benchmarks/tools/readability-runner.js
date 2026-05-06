const { Readability } = require("@mozilla/readability");
const { JSDOM } = require("jsdom");

const TIMEOUT_MS = 30_000;

function extract(html, url) {
    const dom = new JSDOM(html, { url });
    return new Readability(dom.window.document).parse()?.textContent || "";
}

async function fetchText(url) {
    try {
        const r = await fetch(url, { signal: AbortSignal.timeout(TIMEOUT_MS) });
        if (!r.ok) throw new Error(`HTTP ${r.status}`);
        return extract(await r.text(), url);
    } catch (e) {
        process.stderr.write(`readability-runner: ${url}: ${e.stack ?? e.message}\n`);
        return "";
    }
}

async function main() {
    const urls = process.argv.slice(2);
    if (!urls.length) {
        process.stderr.write("usage: readability-runner.js <url>...\n");
        process.exit(2);
    }
    const outs = await Promise.all(urls.map(fetchText));
    process.stdout.write(outs.join("\n\f\n") + "\n");
}

main().catch((e) => {
    process.stderr.write(`readability-runner: ${e.stack ?? e.message}\n`);
    process.exit(1);
});
