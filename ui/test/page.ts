/* Just enough of a page for layout to read its text sizes from the stylesheet. The sizes
 * are what base.css sets, in px, since no stylesheet is loaded here. */
const SIZES: Record<string, string> = {
  "--text": "14px",
  "--text-small": `${14 * 0.9}px`,
};

Object.assign(globalThis, {
  document: { documentElement: {} },
  getComputedStyle: () => ({
    getPropertyValue: (name: string) => SIZES[name] ?? "",
  }),
});
