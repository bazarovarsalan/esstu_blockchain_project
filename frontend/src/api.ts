import type {
  Block,
  ConsensusAttempt,
  IntegrityOverview,
  NetworkSnapshot,
  ScenarioReport,
  Transaction,
} from "./types";

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    ...init,
    headers: {
      "Content-Type": "application/json",
      ...init?.headers,
    },
  });
  if (!response.ok) {
    const body = (await response.json().catch(() => null)) as
      | { message?: string }
      | null;
    throw new Error(body?.message ?? `HTTP ${response.status}`);
  }
  if (response.status === 204 || response.headers.get("content-length") === "0") {
    return undefined as T;
  }
  return response.json() as Promise<T>;
}

export const api = {
  state: () => request<NetworkSnapshot>("/api/state"),
  integrity: () => request<IntegrityOverview>("/api/integrity"),
  block: (height: number) => request<Block>(`/api/blocks/${height}`),
  createTransaction: (payload: {
    sender: string;
    recipient: string;
    amount: number;
  }) =>
    request<Transaction>("/api/transactions", {
      method: "POST",
      body: JSON.stringify(payload),
    }),
  produceBlock: () =>
    request<ConsensusAttempt>("/api/consensus/produce", { method: "POST" }),
  validatorStatus: (id: string, active: boolean) =>
    request<NetworkSnapshot>(`/api/validators/${id}/status`, {
      method: "POST",
      body: JSON.stringify({ active }),
    }),
  reset: () => request<NetworkSnapshot>("/api/demo/reset", { method: "POST" }),
  scenario: (name: string) =>
    request<ScenarioReport>(`/api/demo/scenarios/${name}`, { method: "POST" }),
};

