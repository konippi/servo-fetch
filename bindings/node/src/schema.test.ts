import { describe, expect, it } from "vitest";
import { schemaToJson } from "./schema.js";

describe("schemaToJson", () => {
  it("renames baseSelector to base_selector and preserves fields", () => {
    const json = JSON.parse(
      schemaToJson({
        baseSelector: ".product",
        fields: [
          { name: "title", selector: "h2", type: "text" },
          { name: "url", selector: "a", type: "attribute", attribute: "href" },
        ],
      }),
    );
    expect(json.base_selector).toBe(".product");
    expect(json.fields[1]).toEqual({
      name: "url",
      selector: "a",
      type: "attribute",
      attribute: "href",
    });
  });
});
