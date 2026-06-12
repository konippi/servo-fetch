#!/usr/bin/env node
import { appendFileSync } from "node:fs";

const args = process.argv.slice(2);
const line = args.join(" ");
const has = (token) => args.includes(token);
const write = (s) => process.stdout.write(s);

if (process.env.SERVO_FETCH_ARGS_FILE) {
  appendFileSync(process.env.SERVO_FETCH_ARGS_FILE, `${line}\n`);
}

if (line.includes("badjson")) {
  write("<<<not json>>>\n");
} else if (line.includes("failurl")) {
  process.stderr.write("error: invalid URL 'x'\n");
  process.exitCode = 64; // EX_USAGE; set (not process.exit) so streams flush
} else if (has("--version")) {
  write("servo-fetch 9.9.9\n");
} else if (has("crawl")) {
  write(
    '{"type":"page","url":"https://e.com/","depth":0,"fetchedAt":"2024-01-01T00:00:00.000Z","title":"Home","content":"# Home","linksFound":2}\n',
  );
  write(
    '{"type":"error","url":"https://e.com/bad","depth":1,"fetchedAt":"2024-01-01T00:00:01.000Z","error":"boom"}\n',
  );
  write('{"type":"stats","crawled":2,"errors":1,"elapsedMs":5}\n');
} else if (has("map")) {
  write('[{"url":"https://e.com/"},{"url":"https://e.com/a","lastmod":"2024-01-01"}]\n');
} else if (has("--schema")) {
  write('{"url":"https://e.com/","extracted":[{"title":"x"}]}\n');
} else if (has("--js")) {
  write("JS_RESULT\n");
} else if (has("json")) {
  write('{"title":"T","content":"<p>c</p>","textContent":"c","byline":"me"}\n');
} else if (has("png")) {
  write("PNGDATA");
} else {
  write("# Title\n\nbody\n");
}
