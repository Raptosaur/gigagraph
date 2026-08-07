import axios from "axios";
import got from "got";
import ky from "ky";
import superagent from "superagent";

const api = axios.create({ baseURL: "https://svc.example.com/api" });

export async function listTasks(): Promise<unknown> {
  const res = await api.get("/tasks");
  return res.data;
}

export async function pushGizmo(): Promise<void> {
  await got.post("https://got.example.com/gizmos");
  await ky.get("/kites");
  await superagent.get("/sprockets");
}
