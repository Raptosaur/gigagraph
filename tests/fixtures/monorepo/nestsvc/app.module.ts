import { Module } from "@nestjs/common";
import { DispatchService } from "./dispatch.service";
import { AlertNotifier, EmailNotifier, Notifier } from "./notifiers";

@Module({
  imports: [],
  providers: [
    DispatchService,
    { provide: Notifier, useClass: EmailNotifier },
  ],
})
export class AppModule {}
