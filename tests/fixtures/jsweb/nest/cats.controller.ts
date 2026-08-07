import { Controller, Get, Post } from '@nestjs/common';

@Controller('cats')
export class CatsController {
  @Get(':id')
  findOne() {
    return {};
  }

  @Post()
  create() {
    return {};
  }
}
