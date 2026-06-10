export type Field =
  | { name: string; selector: string; type: "text" | "html" | "inner_html" }
  | { name: string; selector: string; type: "attribute"; attribute: string }
  | { name: string; selector: string; type: "nested_list"; fields: Field[] };

/** Declarative CSS-selector schema for structured JSON extraction. */
export interface Schema {
  /** Repeated container selector; each match produces one object. */
  baseSelector?: string;
  /** Fields to read from each container. */
  fields: Field[];
}

export function schemaToJson(schema: Schema): string {
  return JSON.stringify({ base_selector: schema.baseSelector, fields: schema.fields });
}
