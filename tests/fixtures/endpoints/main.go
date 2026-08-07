package main

import "net/http"

func health(w http.ResponseWriter, r *http.Request) {}

func main() {
	http.HandleFunc("GET /health", health)
	http.HandleFunc("/webhook", nil)
	http.ListenAndServe(":8080", nil)
}
