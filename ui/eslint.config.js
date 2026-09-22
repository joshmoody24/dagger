import js from "@eslint/js";
import solid from "eslint-plugin-solid/configs/typescript";
import ts from "typescript-eslint";

/* What the compiler can't see.
 *
 * Types now cover the shapes; these cover the mistakes that are well-typed and still wrong.
 * Almost all of that is Solid: a component's props only stay live while they're read
 * through the object, so pulling a field out of one — destructuring it, handing it to a
 * plain variable — quietly turns a value that updates into a value that doesn't. It type
 * checks, it renders once, and it never changes again.
 *
 * Nothing here is about how code looks. A lint that argues about style is a lint people
 * learn to ignore, and then it isn't there for the one that matters.
 */
export default ts.config(
  { ignores: ["dist", "src-tauri", "src/types.generated.ts", "*.config.js"] },
  js.configs.recommended,
  ...ts.configs.recommendedTypeChecked,
  solid,
  {
    languageOptions: {
      parserOptions: {
        projectService: true,
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: {
      /* An underscore says the argument is there for its position, not its value. */
      "@typescript-eslint/no-unused-vars": [
        "error",
        { argsIgnorePattern: "^_" },
      ],

      /* A promise nobody waits for fails silently. `void` says the throw is accounted for;
       * leaving it off means it isn't. */
      "@typescript-eslint/no-floating-promises": "error",
      "@typescript-eslint/no-misused-promises": "error",

      /* `==` against null is how you ask "null or undefined", and it's the only place the
       * loose comparison says something the strict one doesn't. */
      eqeqeq: ["error", "always", { null: "ignore" }],

      /* Reading an event's target or a theme's colours means talking to something the
       * compiler can't describe. Worth a cast; not worth failing over. */
      "@typescript-eslint/no-explicit-any": "off",
      "@typescript-eslint/no-unsafe-assignment": "off",
      "@typescript-eslint/no-unsafe-member-access": "off",
      "@typescript-eslint/no-unsafe-argument": "off",
      "@typescript-eslint/no-unsafe-call": "off",
      "@typescript-eslint/no-unsafe-return": "off",

      /* A string in a template is a string. */
      "@typescript-eslint/restrict-template-expressions": "off",

      /* A ref is assigned by the framework, through the JSX, where neither of these can
       * see it happen — so both read every ref in the page as a variable nobody ever set. */
      "no-unassigned-vars": "off",
      "@typescript-eslint/unbound-method": "off",
    },
  },
  {
    /* Tests reach past what the page would, on purpose: a fixture is known good, so
     * asserting on it beats teaching the compiler what it holds. */
    files: ["test/**"],
    rules: {
      "@typescript-eslint/no-non-null-assertion": "off",
      /* node:test hands back a promise for the runner, not for the caller. */
      "@typescript-eslint/no-floating-promises": "off",
    },
  },
);
