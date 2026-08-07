import { Module } from '@nestjs/common';
import { RouterModule } from '@nestjs/core';
import { AdminModule } from './admin/admin.module';
import { CatsController } from './cats.controller';
import { DualController } from './dual.controller';

@Module({
  imports: [
    AdminModule,
    RouterModule.register([{ path: 'admin', module: AdminModule }]),
  ],
  controllers: [CatsController, DualController],
})
export class AppModule {}
