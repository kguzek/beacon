import { solvro } from "@solvro/config/eslint";

// Solvro's type-aware TypeScript rules; Vue SFCs are checked by vue-tsc.
export default solvro(
  {
    files: ["**/*.ts"],
    rules: {
      // Keep ES2022 browser support; these newer built-ins are not universal.
      "unicorn/prefer-iterator-to-array": "off",
      "unicorn/no-array-sort": "off",
      "unicorn/max-nested-calls": ["error", { max: 8 }],
      // Validate identities without silently trimming publisher data.
      "zod/prefer-string-schema-with-trim": "off",
    },
  },
  {
    files: ["tests/**/*.ts"],
    rules: { "@typescript-eslint/no-non-null-assertion": "off" },
  },
);
