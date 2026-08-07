function sendEcho() {
  const xhr = new XMLHttpRequest();
  xhr.open("POST", "/echo");
  xhr.send();
}

function pollBoards() {
  $.ajax({ url: "/ping", type: "GET" });
  $.get("/dashboard");
  jQuery.post("/mixed");
}
