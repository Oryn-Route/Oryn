'use client';

import { useMemo } from 'react';

import { useWallet } from '@/components/providers/wallet-provider';
import { createOrynRouteClient } from '@/lib/api/client';
import { getApiBaseUrl } from '@/lib/network-endpoints';

export function useOrynRouteClient() {
  const { network } = useWallet();
  return useMemo(
    () => createOrynRouteClient(getApiBaseUrl(network)),
    [network],
  );
}
