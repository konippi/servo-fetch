import { expectTypeOf } from "vitest";
import type { Article, Session } from "../src/index.js";

declare const session: Session;

const markdownOnly = await session.fetch("https://e.com");
expectTypeOf(markdownOnly.markdown).toEqualTypeOf<string>();
expectTypeOf(markdownOnly).not.toHaveProperty("html");

const page = await session.fetch("https://e.com", { formats: ["html", "article"] as const });
expectTypeOf(page.html).toEqualTypeOf<string>();
expectTypeOf(page.article).toEqualTypeOf<Article>();
expectTypeOf(page).not.toHaveProperty("markdown");

// @ts-expect-error unknown formats are rejected
await session.fetch("https://e.com", { formats: ["png"] });
