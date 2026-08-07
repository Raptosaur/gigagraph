import { Controller, Get, Version } from '@nestjs/common';

// Multi-path controller (one route set per prefix) plus a URI-versioned
// method (enableVersioning({type: URI}) in main.ts).
@Controller(['bulk', 'batch'])
export class DualController {
  @Get('jobs')
  jobs() {
    return [];
  }

  @Version('1')
  @Get('status')
  status() {
    return {};
  }
}
