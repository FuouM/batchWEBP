import { render } from "solid-js/web";
import "soulwindow/css";
import "bootstrap-icons/font/bootstrap-icons.css";
import "./app.css";
import "./theme";
import { App } from "./App";

render(() => <App />, document.getElementById("app")!);
