import { describe, it, expect } from 'vitest';
import { relativeTime } from '../src/time';

describe('relativeTime', () => {
  const now = new Date('2026-03-19T12:00:00Z').getTime();

  it('returns "just now" for < 10 seconds', () => {
    const date = new Date(now - 5000).toISOString();
    expect(relativeTime(date, now)).toBe('just now');
  });

  it('returns seconds for < 60 seconds', () => {
    const date = new Date(now - 30000).toISOString();
    expect(relativeTime(date, now)).toBe('30s ago');
  });

  it('returns minutes for < 60 minutes', () => {
    const date = new Date(now - 5 * 60 * 1000).toISOString();
    expect(relativeTime(date, now)).toBe('5m ago');
  });

  it('returns hours for < 24 hours', () => {
    const date = new Date(now - 3 * 60 * 60 * 1000).toISOString();
    expect(relativeTime(date, now)).toBe('3h ago');
  });

  it('returns days for < 30 days', () => {
    const date = new Date(now - 7 * 24 * 60 * 60 * 1000).toISOString();
    expect(relativeTime(date, now)).toBe('7d ago');
  });

  it('returns localized date for > 30 days', () => {
    const date = new Date(now - 60 * 24 * 60 * 60 * 1000).toISOString();
    const result = relativeTime(date, now);
    // Should be a date string, not relative
    expect(result).not.toContain('ago');
    expect(result).not.toBe('just now');
  });

  it('handles future dates gracefully (returns "just now")', () => {
    const date = new Date(now + 60000).toISOString();
    expect(relativeTime(date, now)).toBe('just now');
  });

  it('returns exactly at boundaries', () => {
    // Exactly 10 seconds
    expect(relativeTime(new Date(now - 10000).toISOString(), now)).toBe('10s ago');
    // Exactly 60 seconds
    expect(relativeTime(new Date(now - 60000).toISOString(), now)).toBe('1m ago');
    // Exactly 60 minutes
    expect(relativeTime(new Date(now - 3600000).toISOString(), now)).toBe('1h ago');
    // Exactly 24 hours
    expect(relativeTime(new Date(now - 86400000).toISOString(), now)).toBe('1d ago');
  });
});
