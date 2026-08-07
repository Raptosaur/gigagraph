// Fiber: variable-bound groups (nested), a slash-less Group path, and a
// chained `.Use(...)` binding where the group segment sits in the inner
// Group call of the chain.
package main

import (
	"github.com/gofiber/fiber/v2"
)

func listUsers(c *fiber.Ctx) error { return nil }

func auth(c *fiber.Ctx) error { return nil }

func main() {
	app := fiber.New()

	app.Get("/healthz", nil)

	api := app.Group("/api")
	v1 := api.Group("/v1")
	v1.Get("/users", listUsers)

	ping := app.Group("ping")
	ping.Get("/pong", nil)

	todo := app.Group("/todo").Use(auth)
	todo.Get("/list", nil)
	todo.Post("/create", nil)

	app.Listen(":3000")
}
