import { Module } from '@nestjs/common';
import { AdminController } from './admin.controller';
import { AuditController } from './audit.controller';

// Registered at the 'admin' path prefix by RouterModule.register in
// app.module.ts; the multi-member controllers array arrives
// `controllers`-keyed from the harvester.
@Module({ controllers: [AdminController, AuditController] })
export class AdminModule {}
