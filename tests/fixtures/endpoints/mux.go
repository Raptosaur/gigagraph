package muxdemo

import (
	"net/http"

	"github.com/gorilla/mux"
)

func taskShow(w http.ResponseWriter, r *http.Request) {}

func taskCreate(w http.ResponseWriter, r *http.Request) {}

func wire() {
	r := mux.NewRouter()
	r.HandleFunc("/tasks/{id}", taskShow).Methods("GET")
	r.HandleFunc("/tasks", taskCreate).Methods("POST")
	http.ListenAndServe(":8081", r)
}
