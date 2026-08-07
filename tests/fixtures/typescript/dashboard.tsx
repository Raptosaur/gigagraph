import { useEffect, useState } from "react";
import { formatCount } from "./validators";

interface DashboardProps {
  title: string;
  refreshMs: number;
}

export function Dashboard({ title, refreshMs }: DashboardProps) {
  const [count, setCount] = useState(0);

  useEffect(() => {
    const timer = setInterval(() => setCount((c) => c + 1), refreshMs);
    return () => clearInterval(timer);
  }, [refreshMs]);

  return (
    <section>
      <h1>{title}</h1>
      <Badge label={formatCount(count)} onReset={() => setCount(0)} />
    </section>
  );
}

const Badge = ({ label, onReset }: { label: string; onReset: () => void }) => (
  <button onClick={() => onReset()}>{label}</button>
);

export class PanelController {
  refresh = (): void => {
    render(<Dashboard title="panel" refreshMs={1000} />);
  };

  buildLegacy(): string {
    return legacyMarkup("panel");
  }
}

function legacyMarkup(kind: string): string {
  return `<div class="${kind}"></div>`;
}

declare function render(node: unknown): void;
