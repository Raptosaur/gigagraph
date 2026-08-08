import React, { useCallback, useState } from "react";
import { trackEvent } from "./analytics";

interface Props {
  isbns: string[];
  onCheckout: (total: number) => void;
}

export function CartPanel({ isbns, onCheckout }: Props) {
  const [count, setCount] = useState(isbns.length);

  const handleCheckout = useCallback(() => {
    trackEvent("checkout", { count });
    onCheckout(count);
  }, [count, onCheckout]);

  return (
    <div className="cart">
      <span>{count}</span>
      <button onClick={handleCheckout}>Checkout</button>
      <button onClick={() => setCount(count + 1)}>Add</button>
    </div>
  );
}

export const EmptyCart = () => <p>Your cart is empty.</p>;

export default function CartPage(props: Props) {
  return <CartPanel {...props} />;
}
