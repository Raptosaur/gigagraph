// Echo: group variables with empty-path registrations and nested groups.
package main

import (
	"github.com/labstack/echo/v4"
)

func adminHome(c echo.Context) error { return nil }

func getAdminUser(c echo.Context) error { return nil }

func main() {
	e := echo.New()

	e.GET("/", nil)

	g := e.Group("/manage")
	g.GET("", adminHome)

	users := g.Group("/users")
	users.GET("/:id", getAdminUser)

	e.Start(":1323")
}
