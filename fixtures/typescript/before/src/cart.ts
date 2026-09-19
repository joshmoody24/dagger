import { addMoney, Money } from "./money.js";

export function cartTotal(items: Money[]): Money {
  return items.reduce(addMoney);
}
