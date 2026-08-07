import { Notifier } from "./notifiers";

export class DispatchService {
  constructor(private notifier: Notifier) {}

  broadcast(msg: string): void {
    this.notifier.send(msg);
  }
}
