import { NativeModules } from "react-native";

const { Analytics } = NativeModules;

export async function payNow(amount: number): Promise<void> {
  await NativeModules.Payments.charge(amount);
}

export function logCheckout(): void {
  Analytics.track("checkout");
}

export function callMissing(): void {
  NativeModules.Gone.vanish();
}

export async function whereAmI(): Promise<void> {
  await NativeModules.Location.locate("fine");
}

export function pingGeo(): void {
  NativeModules.Geo2.ping(51.5);
}

export function showBadge(): void {
  NativeModules.SwiftBadge.show(3);
}

export function readSpeed(): void {
  NativeModules.Speed.speed(88);
}
