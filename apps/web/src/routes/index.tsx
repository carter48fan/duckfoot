import { createFileRoute } from "@tanstack/react-router";
import { CreativeSuite } from "../components/suite/CreativeSuite";

export const Route = createFileRoute("/")({ component: Home });

function Home() {
  return <CreativeSuite />;
}
