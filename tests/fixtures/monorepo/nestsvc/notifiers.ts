export abstract class Notifier {
  abstract send(msg: string): void;
}

// Declared BEFORE EmailNotifier so that, without the @Module provider
// binding, bare hierarchy expansion would rank this implementor first
// (smaller fn id) — the integration test would then fail, proving the
// {provide, useClass} pair is load-bearing.
export class AlertNotifier extends Notifier {
  send(msg: string): void {
    console.log("alert: " + msg);
  }
}

export class EmailNotifier extends Notifier {
  send(msg: string): void {
    console.log("email: " + msg);
  }
}
