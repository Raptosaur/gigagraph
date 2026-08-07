// Gin group-prefix composition: variable-bound groups, nested groups, and
// cross-file register functions (realworld-app shape).
package main

import (
	"github.com/gin-gonic/gin"

	"example.com/ginweb/users"
)

func ping(c *gin.Context) {}

func main() {
	r := gin.Default()

	v1 := r.Group("/api")
	users.UsersRegister(v1.Group("/users"))

	admin := v1.Group("/admin")
	admin.GET("/stats", ping)

	testAuth := r.Group("/api/ping")
	testAuth.GET("/", ping)
}
