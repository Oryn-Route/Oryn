import { render, screen, cleanup } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { Footer } from './footer';
import { vi } from 'vitest';
import { describe, it, expect, afterEach } from 'vitest';

const setNetwork = vi.fn();

vi.mock('@/components/providers/wallet-provider', () => ({
  useWallet: () => ({
    network: 'testnet',
    setNetwork,
  }),
}));

vi.mock('@/lib/network-policy', () => ({
  getAllowedNetworks: vi.fn(() => ['testnet']),
  NETWORK_LABELS: { testnet: 'Testnet', mainnet: 'Mainnet' },
}));

vi.mock('next/link', () => ({
  default: ({ children, href }: { children: React.ReactNode; href: string }) => (
    <a href={href}>{children}</a>
  ),
}));

describe('Footer', () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it('renders all footer internal links', () => {
    render(<Footer />);

    expect(screen.getByRole('link', { name: /^Swap$/i })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Cross-chain/i })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Stellar DEX/i })).toBeInTheDocument();
  });

  it('links landing pages as internal routes', () => {
    render(<Footer />);

    expect(screen.getByRole('link', { name: /Cross-chain/i })).toHaveAttribute(
      'href',
      '/cross-chain-swap',
    );
    expect(screen.getByRole('link', { name: /Stellar DEX/i })).toHaveAttribute(
      'href',
      '/stellar-dex-aggregator',
    );
  });

  it('renders network links only once for single network', () => {
    render(<Footer />);

    const redundantLinks = screen.queryByRole('link', { name: /Status/i });
    expect(redundantLinks).not.toBeInTheDocument();
  });
});
