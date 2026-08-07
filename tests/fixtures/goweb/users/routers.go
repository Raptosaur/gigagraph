package users

import "github.com/gin-gonic/gin"

func UsersRegistration(c *gin.Context) {}

func UsersLogin(c *gin.Context) {}

func UsersRegister(router *gin.RouterGroup) {
	router.POST("", UsersRegistration)
	router.POST("/login", UsersLogin)
}
