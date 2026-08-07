package gosvc

import (
	"fmt"
	"net/http"
)

func handleOrders(w http.ResponseWriter, r *http.Request) {
	fmt.Fprint(w, Normalize("orders"))
}

func StartHttp(addr string) {
	http.HandleFunc("/orders", handleOrders)
	http.ListenAndServe(addr, nil)
}
