import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { cn, flyAndScale } from './index';

describe('cn', () => {
  it('returns an empty string when called with no arguments', () => {
    expect(cn()).toBe('');
  });

  it('returns a single class name unchanged', () => {
    expect(cn('text-red-500')).toBe('text-red-500');
  });

  it('joins multiple class names', () => {
    expect(cn('a', 'b', 'c')).toBe('a b c');
  });

  it('filters out falsy values', () => {
    expect(cn('a', false && 'b', undefined, null, 'c')).toBe('a c');
  });

  it('handles conditional class (truthy)', () => {
    const active = true;
    expect(cn('base', active && 'active')).toBe('base active');
  });

  it('handles conditional class (falsy)', () => {
    const active = false;
    expect(cn('base', active && 'active')).toBe('base');
  });

  it('resolves Tailwind conflicting utility — last wins', () => {
    // tailwind-merge removes the earlier conflicting utility.
    expect(cn('p-4', 'p-2')).toBe('p-2');
  });

  it('resolves Tailwind padding conflict across axes', () => {
    expect(cn('px-4', 'px-2')).toBe('px-2');
  });

  it('merges without conflict when utilities are on different axes', () => {
    const result = cn('px-4', 'py-2');
    expect(result).toContain('px-4');
    expect(result).toContain('py-2');
  });

  it('handles object syntax from clsx', () => {
    expect(cn({ 'text-red-500': true, 'text-blue-500': false })).toBe('text-red-500');
  });

  it('handles array syntax from clsx', () => {
    expect(cn(['a', 'b'])).toBe('a b');
  });

  it('handles nested arrays from clsx', () => {
    expect(cn(['a', ['b', 'c']])).toBe('a b c');
  });

  it('handles mixed string and object input', () => {
    expect(cn('base', { 'extra': true })).toBe('base extra');
  });

  it('handles Tailwind text color override', () => {
    expect(cn('text-red-500', 'text-blue-500')).toBe('text-blue-500');
  });

  it('handles Tailwind background color override', () => {
    expect(cn('bg-red-500', 'bg-blue-500')).toBe('bg-blue-500');
  });

  it('does not drop non-conflicting utilities', () => {
    const result = cn('text-red-500', 'font-bold');
    expect(result).toContain('text-red-500');
    expect(result).toContain('font-bold');
  });
});

// ── flyAndScale ───────────────────────────────────────────────────────────────
// getComputedStyle is a DOM global that doesn't exist in node environment.
// We stub it out so the pure-logic paths in flyAndScale can be exercised.

describe('flyAndScale', () => {
  let mockNode: Element;

  beforeEach(() => {
    // Provide a minimal getComputedStyle stub that returns transform: 'none'
    global.getComputedStyle = vi.fn().mockReturnValue({ transform: 'none' });
    mockNode = {} as Element;
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('returns a TransitionConfig with default params', () => {
    const config = flyAndScale(mockNode);
    // Default params object has duration: 150; the ?? 200 fallback only fires
    // when params.duration is explicitly undefined.
    expect(config.duration).toBe(150);
    expect(config.delay).toBe(0);
    expect(typeof config.css).toBe('function');
    expect(config.easing).toBeDefined();
  });

  it('uses custom duration', () => {
    const config = flyAndScale(mockNode, { duration: 400 });
    expect(config.duration).toBe(400);
  });

  it('falls back to 200ms when duration is not supplied in params', () => {
    const config = flyAndScale(mockNode, { y: -8 });
    expect(config.duration).toBe(200);
  });

  it('css(t=0) produces opacity 0 and starting scale', () => {
    const config = flyAndScale(mockNode, { y: 10, x: 0, start: 0.9, duration: 150 });
    const css = config.css!(0, 1);
    expect(css).toContain('opacity:0');
    expect(css).toContain('scale(0.9)');
  });

  it('css(t=1) produces opacity 1 and scale 1', () => {
    const config = flyAndScale(mockNode, { y: 10, x: 5, start: 0.8, duration: 150 });
    const css = config.css!(1, 0);
    expect(css).toContain('opacity:1');
    expect(css).toContain('scale(1)');
    // At t=1 both y and x converge to 0
    expect(css).toContain('translate3d(0px, 0px, 0)');
  });

  it('preserves existing transform when not "none"', () => {
    global.getComputedStyle = vi.fn().mockReturnValue({ transform: 'rotate(45deg)' });
    const config = flyAndScale(mockNode);
    const css = config.css!(1, 0);
    expect(css).toContain('rotate(45deg)');
  });

  it('css returns a string (styleToString round-trip)', () => {
    const config = flyAndScale(mockNode, { y: -8, x: 0, start: 0.95, duration: 150 });
    const css = config.css!(0.5, 0.5);
    expect(typeof css).toBe('string');
    expect(css).toContain(';');
  });

  it('uses ?? fallback defaults when params has no y/x/start', () => {
    // Passing an empty params object triggers the ?? fallback values:
    // y defaults to 5, x defaults to 0, start defaults to 0.95
    const config = flyAndScale(mockNode, {});
    const css = config.css!(0, 1);
    // At t=0 the scale should be 0.95 (the ?? fallback for start)
    expect(css).toContain('scale(0.95)');
    // opacity should be 0 at t=0
    expect(css).toContain('opacity:0');
  });
});
