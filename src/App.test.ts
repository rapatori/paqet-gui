import { render, screen } from '@testing-library/svelte';
import App from './App.svelte';

describe('application shell', () => {
  it('renders an honest disconnected placeholder state', () => {
    render(App);

    expect(
      screen.getByRole('heading', { level: 1, name: 'paqet' }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText('Connection status')).toHaveTextContent(
      'Disconnected',
    );
    expect(screen.getByRole('button', { name: 'Connect' })).toBeDisabled();
    expect(
      screen.getByRole('log', { name: 'Connection logs' }),
    ).toHaveTextContent('Connection output will appear here.');
  });
});
