import { mount } from 'svelte'
import './app.css'
import App from './App.svelte'
import { applyOsClass } from './lib/platform'

applyOsClass() // marca <html> con is-mac/is-windows/is-linux para los estilos por OS

const app = mount(App, { target: document.getElementById('app')! })

export default app
