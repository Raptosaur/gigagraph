// User wrapper convention over gorilla (go-todo-rest-api shape): the real
// routes are the wrapper CALL SITES (`a.Get("/projects", ...)`) — the
// HandleFunc inside each wrapper has a variable path and stays invisible.
// Handlers live in another package and resolve only by global uniqueness.
package wrap

import (
	"net/http"

	"github.com/gorilla/mux"

	"example.com/goweb/handler"
)

type App struct {
	Router *mux.Router
}

func (a *App) setRouters() {
	a.Get("/projects", a.handleRequest(handler.GetAllProjects))
	a.Post("/projects", a.handleRequest(handler.CreateProject))
	a.Put("/projects/{title}", a.handleRequest(handler.UpdateProject))
}

func (a *App) Get(path string, f func(w http.ResponseWriter, r *http.Request)) {
	a.Router.HandleFunc(path, f).Methods("GET")
}

func (a *App) Post(path string, f func(w http.ResponseWriter, r *http.Request)) {
	a.Router.HandleFunc(path, f).Methods("POST")
}

func (a *App) Put(path string, f func(w http.ResponseWriter, r *http.Request)) {
	a.Router.HandleFunc(path, f).Methods("PUT")
}

func (a *App) handleRequest(h func(w http.ResponseWriter, r *http.Request)) func(w http.ResponseWriter, r *http.Request) {
	return h
}
