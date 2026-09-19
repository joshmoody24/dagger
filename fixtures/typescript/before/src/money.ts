export type Currency = "USD" | "EUR";

export interface Money {
  amount: number;
  currency: Currency;
}

export function addMoney(a: Money, b: Money): Money {
  return { amount: a.amount + b.amount, currency: a.currency };
}
