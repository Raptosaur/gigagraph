package main

import (
	"encoding/json"
	"log"
	"net/http"

	"example.com/warehouse/internal/inventory"
)

var svc = inventory.NewService()

func main() {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /health", handleHealth)
	mux.HandleFunc("GET /v1/items/{sku}", handleGetItem)
	mux.HandleFunc("POST /v1/items/{sku}/reserve", handleReserve)
	log.Fatal(http.ListenAndServe(":8080", mux))
}

func handleHealth(w http.ResponseWriter, r *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
}

func handleGetItem(w http.ResponseWriter, r *http.Request) {
	sku := r.PathValue("sku")
	writeJSON(w, http.StatusOK, map[string]string{"sku": sku})
}

func handleReserve(w http.ResponseWriter, r *http.Request) {
	if err := svc.Reserve(r.PathValue("sku"), 1); err != nil {
		writeJSON(w, http.StatusConflict, map[string]string{"error": err.Error()})
		return
	}
	w.WriteHeader(http.StatusNoContent)
}

func writeJSON(w http.ResponseWriter, code int, body any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	_ = json.NewEncoder(w).Encode(body)
}
