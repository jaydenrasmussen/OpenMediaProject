import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import * as fc from "fast-check";
import type { ClassValue } from "clsx";
import { cn, flyAndScale } from "./index";

// ── cn ────────────────────────────────────────────────────────────────────────

describe("cn — property-based", () => {
    // Property: cn never throws for any combination of valid ClassValue inputs.
    it("never throws for any combination of ClassValue inputs", () => {
        fc.assert(
            fc.property(
                fc.array(
                    fc.oneof(
                        fc.string(),
                        fc.constant(false as const),
                        fc.constant(null),
                        fc.constant(undefined),
                        fc.dictionary(fc.string(), fc.boolean()),
                    ),
                ),
                (inputs) => {
                    expect(() => cn(...(inputs as ClassValue[]))).not.toThrow();
                },
            ),
        );
    });

    // Property: cn always returns a string.
    it("always returns a string", () => {
        fc.assert(
            fc.property(fc.array(fc.string()), (inputs) => {
                expect(typeof cn(...inputs)).toBe("string");
            }),
        );
    });

    // Property: cn is idempotent — calling it twice with the same string
    // produces the same result.
    it("is idempotent for any single string input", () => {
        fc.assert(
            fc.property(fc.string(), (s) => {
                expect(cn(s)).toBe(cn(s));
            }),
        );
    });

    // Property: non-conflicting Tailwind utilities are both present in the result.
    // text-* and font-* utilities occupy different CSS properties and never conflict.
    it("preserves both classes when utility prefixes do not conflict", () => {
        fc.assert(
            fc.property(
                fc.constantFrom(
                    "text-red-500",
                    "text-blue-500",
                    "text-green-500",
                    "text-yellow-500",
                ),
                fc.constantFrom(
                    "font-bold",
                    "font-semibold",
                    "font-light",
                    "font-medium",
                ),
                (textClass, fontClass) => {
                    const result = cn(textClass, fontClass);
                    expect(result).toContain(textClass);
                    expect(result).toContain(fontClass);
                },
            ),
        );
    });

    // Property: when two padding utilities conflict, the later one wins and the
    // earlier one is removed (tailwind-merge conflict resolution).
    it("last padding class wins when two padding utilities conflict", () => {
        const paddings = ["p-1", "p-2", "p-3", "p-4", "p-6", "p-8"] as const;
        fc.assert(
            fc.property(
                fc.constantFrom(...paddings),
                fc.constantFrom(...paddings),
                (first, second) => {
                    const result = cn(first, second);
                    expect(result).toContain(second);
                    if (first !== second) {
                        expect(result).not.toContain(first);
                    }
                },
            ),
        );
    });

    // Property: all-falsy inputs produce an empty string.
    it("returns empty string when all inputs are falsy", () => {
        fc.assert(
            fc.property(
                fc.array(
                    fc.oneof(
                        fc.constant(false as const),
                        fc.constant(null),
                        fc.constant(undefined),
                    ),
                    { minLength: 1 },
                ),
                (inputs) => {
                    expect(cn(...(inputs as ClassValue[]))).toBe("");
                },
            ),
        );
    });
});

// ── flyAndScale ───────────────────────────────────────────────────────────────

describe("flyAndScale — property-based", () => {
    beforeEach(() => {
        global.getComputedStyle = vi.fn().mockReturnValue({ transform: "none" });
    });

    afterEach(() => {
        vi.restoreAllMocks();
    });

    // Arbitrary for params that exercises all optional fields.
    const paramsArb = fc.record({
        y: fc.double({ noNaN: true }),
        x: fc.double({ noNaN: true }),
        start: fc.double({ min: 0, max: 2, noNaN: true }),
        duration: fc.integer({ min: 0, max: 10_000 }),
    });

    // Property: delay is always 0 regardless of params.
    it("delay is always 0 for any params", () => {
        fc.assert(
            fc.property(paramsArb, (params) => {
                const config = flyAndScale({} as Element, params);
                expect(config.delay).toBe(0);
            }),
        );
    });

    // Property: duration always reflects the value supplied in params.
    it("duration always reflects params.duration", () => {
        fc.assert(
            fc.property(fc.integer({ min: 0, max: 10_000 }), (duration) => {
                const config = flyAndScale({} as Element, { duration });
                expect(config.duration).toBe(duration);
            }),
        );
    });

    // Property: easing is always defined regardless of params.
    it("easing is always defined", () => {
        fc.assert(
            fc.property(
                fc.record({
                    y: fc.option(fc.double({ noNaN: true }), { nil: undefined }),
                    x: fc.option(fc.double({ noNaN: true }), { nil: undefined }),
                    duration: fc.option(fc.integer({ min: 0, max: 5_000 }), {
                        nil: undefined,
                    }),
                }),
                (params) => {
                    const config = flyAndScale({} as Element, params);
                    expect(config.easing).toBeDefined();
                },
            ),
        );
    });

    // Property: css(t) always returns a non-empty string for any t in [0, 1].
    it("css(t) always returns a non-empty string for t in [0, 1]", () => {
        fc.assert(
            fc.property(fc.double({ min: 0, max: 1, noNaN: true }), (t) => {
                const config = flyAndScale({} as Element, {});
                const result = config.css!(t, 1 - t);
                expect(typeof result).toBe("string");
                expect(result.length).toBeGreaterThan(0);
            }),
        );
    });

    // Property: css(t) encodes opacity directly as t.
    // styleToString uses `${key}:${value}` so opacity will always equal t verbatim.
    it("css(t) encodes opacity as t for any t in [0, 1]", () => {
        fc.assert(
            fc.property(fc.double({ min: 0, max: 1, noNaN: true }), (t) => {
                const config = flyAndScale({} as Element);
                const result = config.css!(t, 1 - t);
                expect(result).toContain(`opacity:${t}`);
            }),
        );
    });

    // Property: css(0) always encodes opacity as 0 regardless of params.
    it("css(0) always produces opacity:0 for any params", () => {
        fc.assert(
            fc.property(paramsArb, (params) => {
                const config = flyAndScale({} as Element, params);
                expect(config.css!(0, 1)).toContain("opacity:0");
            }),
        );
    });

    // Property: css(1) always encodes opacity as 1 regardless of params.
    it("css(1) always produces opacity:1 for any params", () => {
        fc.assert(
            fc.property(paramsArb, (params) => {
                const config = flyAndScale({} as Element, params);
                expect(config.css!(1, 0)).toContain("opacity:1");
            }),
        );
    });
});
