import { defineConfig } from "tsdown";

export default defineConfig({
  entry: ["src/index.ts", "src/cli.ts"],
  dts: { entry: "src/index.ts" },
  publint: true,
  attw: { level: "error" },
});
