// DI shapes: typed fields, constructor parameter properties, heritage,
// typed/constructed locals. Exercises the @field/@local/@hier captures.
import * as cdk from "aws-cdk-lib";

export interface Notifier {
  notify(msg: string): void;
}

export interface AuditedNotifier extends Notifier {
  audit(msg: string): void;
}

export abstract class BaseHandler {
  protected abstract queue: JobQueue;
}

export class EmailNotifier extends BaseHandler implements Notifier {
  private transport: SmtpTransport;
  readonly bucket: cdk.Bucket;
  private fallback: any;

  constructor(
    private mailer: Mailer,
    protected readonly clock: Clock,
    readonly tag: TagService,
    plainLimit: RateLimit,
    private retries?: RetryPolicy,
  ) {
    super();
    this.transport = new SmtpTransport();
    this.fallback = new ConsoleNotifier();
  }

  notify(msg: string): void {
    const svc = new AuthService();
    let helper: Helper = makeHelper();
    const stack: cdk.Stack = new cdk.Stack();
    svc.check(msg);
    helper.run(stack);
  }
}

export class InfraStack extends cdk.Stack {}
