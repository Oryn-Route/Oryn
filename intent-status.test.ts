import { describe, it, expect } from 'vitest';
import {
  intentStatusLabel,
  intentStatusBadge,
  canOpenAuction,
  isTerminalIntent,
  roundStatusLabel,
  roundStatusBadge,
  auctionProgressPercent,
  solverStatusLabel,
  solverStatusBadge,
  canSubmitQuotes,
} from './intent-status';

describe('OFA intent-status helpers', () => {
  describe('intentStatusLabel', () => {
    it('returns human-readable label for every status', () => {
      expect(intentStatusLabel('pending')).toBe('Pending');
      expect(intentStatusLabel('auction_open')).toBe('Auction Open');
      expect(intentStatusLabel('auction_won')).toBe('Auction Won');
      expect(intentStatusLabel('filled')).toBe('Filled');
      expect(intentStatusLabel('abandoned')).toBe('No Solver');
      expect(intentStatusLabel('expired')).toBe('Expired');
      expect(intentStatusLabel('cancelled')).toBe('Cancelled');
    });
  });

  describe('intentStatusBadge', () => {
    it('returns a Tailwind class string containing dark: prefix', () => {
      const badge = intentStatusBadge('pending');
      expect(badge).toContain('dark:');
    });
  });

  describe('canOpenAuction', () => {
    it('allows pending only', () => {
      expect(canOpenAuction('pending')).toBe(true);
      expect(canOpenAuction('auction_open')).toBe(false);
      expect(canOpenAuction('filled')).toBe(false);
    });
  });

  describe('isTerminalIntent', () => {
    it('recognises terminal states', () => {
      expect(isTerminalIntent('filled')).toBe(true);
      expect(isTerminalIntent('expired')).toBe(true);
      expect(isTerminalIntent('cancelled')).toBe(true);
      expect(isTerminalIntent('pending')).toBe(false);
      expect(isTerminalIntent('auction_open')).toBe(false);
    });
  });

  describe('roundStatusLabel', () => {
    it('returns human-readable labels', () => {
      expect(roundStatusLabel('open')).toBe('Open');
      expect(roundStatusLabel('closed')).toBe('Judging');
      expect(roundStatusLabel('settled')).toBe('Settled');
      expect(roundStatusLabel('expired')).toBe('Expired');
    });
  });

  describe('auctionProgressPercent', () => {
    it('returns 0 before window opens', () => {
      expect(auctionProgressPercent(1000, 3000, 500)).toBe(0);
    });

    it('returns 100 at or after close', () => {
      expect(auctionProgressPercent(1000, 3000, 3000)).toBe(100);
      expect(auctionProgressPercent(1000, 3000, 4000)).toBe(100);
    });

    it('returns 50 at midpoint', () => {
      expect(auctionProgressPercent(1000, 3000, 2000)).toBe(50);
    });

    it('returns 0 when window has zero/negative duration', () => {
      expect(auctionProgressPercent(3000, 3000, 3000)).toBe(100);
      expect(auctionProgressPercent(3000, 1000, 2000)).toBe(0);
    });
  });

  describe('roundStatusBadge', () => {
    it('returns a Tailwind class string', () => {
      expect(roundStatusBadge('open')).toContain('dark:');
    });
  });

  describe('solverStatusLabel', () => {
    it('returns human-readable labels', () => {
      expect(solverStatusLabel('active')).toBe('Active');
      expect(solverStatusLabel('suspended')).toBe('Suspended');
      expect(solverStatusLabel('slashed')).toBe('Slashed');
    });
  });

  describe('canSubmitQuotes', () => {
    it('allows active only', () => {
      expect(canSubmitQuotes('active')).toBe(true);
      expect(canSubmitQuotes('suspended')).toBe(false);
      expect(canSubmitQuotes('slashed')).toBe(false);
    });
  });

  describe('solverStatusBadge', () => {
    it('returns a Tailwind class string', () => {
      expect(solverStatusBadge('active')).toContain('dark:');
    });
  });
});
