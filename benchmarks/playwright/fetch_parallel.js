// Playwright: load pages in parallel, extract innerText.
const { chromium } = require("playwright");

(async () => {
  const urls = process.argv.slice(2);
  if (!urls.length) process.exit(1);

  const browser = await chromium.launch({ headless: true });
  await Promise.all(
    urls.map(async (url) => {
      const page = await browser.newPage();
      await page.goto(url, { waitUntil: "load" });
      const text = await page.evaluate(() => document.body.innerText);
      console.log(`[${url}] ${text.length} chars`);
      await page.close();
    })
  );
  await browser.close();
})();
