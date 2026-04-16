(function(){let e=document.createElement(`link`).relList;if(e&&e.supports&&e.supports(`modulepreload`))return;for(let e of document.querySelectorAll(`link[rel="modulepreload"]`))n(e);new MutationObserver(e=>{for(let t of e)if(t.type===`childList`)for(let e of t.addedNodes)e.tagName===`LINK`&&e.rel===`modulepreload`&&n(e)}).observe(document,{childList:!0,subtree:!0});function t(e){let t={};return e.integrity&&(t.integrity=e.integrity),e.referrerPolicy&&(t.referrerPolicy=e.referrerPolicy),e.crossOrigin===`use-credentials`?t.credentials=`include`:e.crossOrigin===`anonymous`?t.credentials=`omit`:t.credentials=`same-origin`,t}function n(e){if(e.ep)return;e.ep=!0;let n=t(e);fetch(e.href,n)}})();function e(e){return`
    <main>
      <h1>Hello, World</h1>
      <p>grass-worker initial scaffold</p>
      <p>API: ${e.apiBaseUrl}</p>
      <p>Node: ${e.nodeBaseUrl}</p>
    </main>
  `}var t=document.querySelector(`#app`);if(!t)throw Error(`Missing #app root`);t.innerHTML=e({apiBaseUrl:`http://127.0.0.1:3000`,nodeBaseUrl:`http://127.0.0.1:3001`});