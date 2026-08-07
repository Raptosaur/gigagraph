import { retry } from "./util";

function privateDead(): number {
  return 1;
}

function usedAsCallback(): number {
  return 2;
}

function calledDynamically(): number {
  return 3;
}

export function exportedUnused(): number {
  return 4;
}

function onMessage(): void {}

export function runAll(thing: any): void {
  retry(usedAsCallback);
  thing.calledDynamically();
}
