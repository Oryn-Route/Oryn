import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useTokenPairUrl } from "./useTokenPairUrl";

// Mock Next.js navigation hooks
const mockPush = vi.fn();
const mockSearchParams = new URLSearchParams();
const mockPathname = "/swap";

vi.mock("next/navigation", () => ({
  useRouter: () => ({
    push: mockPush,
  }),
  useSearchParams: () => mockSearchParams,
  usePathname: () => mockPathname,
}));

describe("useTokenPairUrl", () => {
  beforeEach(() => {
    mockPush.mockClear();
    mockSearchParams.delete("base");
    mockSearchParams.delete("quote");
  });

  it("returns undefined for base and quote when no params are set", () => {
    const { result } = renderHook(() => useTokenPairUrl());

    expect(result.current.base).toBeUndefined();
    expect(result.current.quote).toBeUndefined();
  });

  it("reads base and quote from URL params", () => {
    mockSearchParams.set("base", "native");
    mockSearchParams.set("quote", "USDC:ISSUER");

    const { result } = renderHook(() => useTokenPairUrl());

    expect(result.current.base).toBe("native");
    expect(result.current.quote).toBe("USDC:ISSUER");
  });

  it("updates URL when setPair is called", () => {
    const { result } = renderHook(() => useTokenPairUrl());

    act(() => {
      result.current.setPair("native", "USDC:ISSUER");
    });

    expect(mockPush).toHaveBeenCalledWith(
      "/swap?base=native&quote=USDC%3AISSUER",
      { scroll: false }
    );
  });

  it("removes params when empty strings are provided", () => {
    mockSearchParams.set("base", "native");
    mockSearchParams.set("quote", "USDC:ISSUER");

    const { result } = renderHook(() => useTokenPairUrl());

    act(() => {
      result.current.setPair("", "");
    });

    expect(mockPush).toHaveBeenCalledWith("/swap", { scroll: false });
  });

  it("preserves other query params when updating pair", () => {
    mockSearchParams.set("amount", "100");
    mockSearchParams.set("base", "native");

    const { result } = renderHook(() => useTokenPairUrl());

    act(() => {
      result.current.setPair("native", "USDC:ISSUER");
    });

    expect(mockPush).toHaveBeenCalledWith(
      expect.stringContaining("amount=100"),
      { scroll: false }
    );
    expect(mockPush).toHaveBeenCalledWith(
      expect.stringContaining("base=native"),
      { scroll: false }
    );
    expect(mockPush).toHaveBeenCalledWith(
      expect.stringContaining("quote=USDC%3AISSUER"),
      { scroll: false }
    );
  });

  it("sets isInitializing to false after mount", () => {
    const { result } = renderHook(() => useTokenPairUrl());

    // Initially true, then becomes false after effect runs
    expect(result.current.isInitializing).toBe(false);
  });
});
