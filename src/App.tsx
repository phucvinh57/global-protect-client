import { ConnectionPage } from "@/pages/connection";
import { ToastProvider } from "@/providers/toast-provider";

function App() {
	return (
		<ToastProvider>
			<ConnectionPage />
		</ToastProvider>
	);
}

export default App;
