import { mkdir, writeFile } from "node:fs/promises";
import { z } from "zod";

import { currentSchema, historySchema, settingsSchema } from "../src/api.ts";

const directory = new URL("../../docs/schema/", import.meta.url);
await mkdir(directory, { recursive: true });
for (const [name, schema] of Object.entries({
  current: currentSchema,
  history: historySchema,
  locations: settingsSchema,
})) {
  const document = z.toJSONSchema(schema, { io: "input" });
  await writeFile(
    new URL(`${name}.schema.json`, directory),
    `${JSON.stringify(document, null, 2)}\n`,
  );
}
