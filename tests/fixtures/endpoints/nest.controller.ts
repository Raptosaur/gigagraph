import { Controller, Get, Post } from "@nestjs/common";

@Controller("gadgets")
export class GadgetsController {
  @Get(":id")
  getGadget(id: string): string {
    return id;
  }

  @Post()
  createGadget(): void {}
}
