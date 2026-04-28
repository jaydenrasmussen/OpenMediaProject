import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
	plugins: [sveltekit()],
	server: {
		// Configure dev server
		port: 5173,
		strictPort: true,
		// Allow connections from all network interfaces
		host: '0.0.0.0',
		// Proxy API requests in development
		proxy: {
			'/api': {
				target: process.env.VITE_API_URL || 'http://localhost:8080',
				changeOrigin: true,
				rewrite: (path) => path.replace(/^\/api/, '/api'),
				secure: false
			}
		}
	},
	build: {
		// Output location (handled by adapter-static)
		outDir: 'build',
		// Generate sourcemaps for better debugging
		sourcemap: true,
		// Minify for production
		minify: true,
		// Enable optimizations
		cssCodeSplit: true,
	},
	optimizeDeps: {
		include: ['axios']
	}
});
