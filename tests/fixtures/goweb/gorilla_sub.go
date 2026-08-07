// gorilla/mux subrouter chains (mattermost shape): PathPrefix().Subrouter()
// bound to locals AND to struct fields via plain assignment, composing
// nested prefixes; path-builder idents sharing the path's argument index are
// skipped during handler resolution (versionHandler lives in a sibling file
// of the same package).
package main

import (
	"net/http"

	"github.com/gorilla/mux"
)

type baseRoutes struct {
	Users *mux.Router
}

func prefixedPath(p string) string { return p }

func wireSub() {
	r := mux.NewRouter()
	b := &baseRoutes{}

	api := r.PathPrefix("/api/v4").Subrouter()
	api.HandleFunc(prefixedPath("/version"), versionHandler).Methods("GET")

	b.Users = api.PathPrefix("/users").Subrouter()
	b.Users.HandleFunc("", usersRoot).Methods("GET")
	b.Users.HandleFunc("/{id}", getUser2).Methods("GET", "DELETE")

	http.ListenAndServe(":8065", r)
}

func usersRoot(w http.ResponseWriter, r *http.Request) {}

func getUser2(w http.ResponseWriter, r *http.Request) {}
