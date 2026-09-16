import { createFileRoute } from '@tanstack/react-router';
import { CreativeSuite } from '../components/suite/CreativeSuite';

export const Route = createFileRoute('/editor')({ component: Editor });

function Editor() {
  return <CreativeSuite />;
}
