import { describe, expect, it } from "vitest";
import { schemaToValue } from "./schema.js";

describe("schemaToValue", () => {
  it("renames baseSelector to base_selector and preserves fields", () => {
    const value = schemaToValue({
      baseSelector: ".product",
      fields: [
        { name: "title", selector: "h2", type: "text" },
        { name: "url", selector: "a", type: "attribute", attribute: "href" },
      ],
    });
    expect(value.base_selector).toBe(".product");
    expect(value.fields[1]).toEqual({
      name: "url",
      selector: "a",
      type: "attribute",
      attribute: "href",
    });
  });
});
