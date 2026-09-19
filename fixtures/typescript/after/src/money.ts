export type Currency = "USD" | "EUR" | "GBP";

export interface Money {
  amount: number;
  currency: Currency;
  precise: boolean;
}

export function addMoney(a: Money, b: Money): Money {
  if (a.currency !== b.currency) throw new Error("mismatch");
  return { amount: a.amount + b.amount, currency: a.currency, precise: true };
}
