import { Controller, Get } from '@nestjs/common';

@Controller('dash')
export class AdminController {
  @Get('stats')
  stats() {
    return {};
  }
}
