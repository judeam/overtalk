/** @type {import('tailwindcss').Config} */
module.exports = {
    darkMode: ['class'],
    content: [
    './src/pages/**/*.{js,ts,jsx,tsx,mdx}',
    './src/components/**/*.{js,ts,jsx,tsx,mdx}',
    './src/features/**/*.{js,ts,jsx,tsx,mdx}',
    './src/app/**/*.{js,ts,jsx,tsx,mdx}',
  ],
  theme: {
  	extend: {
  		fontFamily: {
  			sans: [
				'var(--font-instrument)', 'Segoe UI', 'sans-serif'
  			]
  		},
  		colors: {
  			background: 'hsl(var(--background))',
            blue: Object.fromEntries([50,100,200,300,400,500,600,700,800,900,950].map(n => [n, `hsl(var(--copper-${n}) / <alpha-value>)`])),
            gray: Object.fromEntries([50,100,200,300,400,500,600,700,800,900,950].map(n => [n, `hsl(var(--neutral-${n}) / <alpha-value>)`])),
            white: '#fff',
            ...Object.fromEntries(['yellow','amber','orange'].map(name => [name, {50:'hsl(var(--warning-surface) / <alpha-value>)',100:'hsl(var(--warning-surface) / <alpha-value>)',200:'hsl(var(--warning-border) / <alpha-value>)',600:'hsl(var(--warning-text) / <alpha-value>)',700:'hsl(var(--warning-text) / <alpha-value>)',800:'hsl(var(--warning-text) / <alpha-value>)'}])),
            red: {50:'hsl(var(--error-surface) / <alpha-value>)',100:'hsl(var(--error-surface) / <alpha-value>)',200:'hsl(var(--error-border) / <alpha-value>)',700:'hsl(var(--error-text) / <alpha-value>)',800:'hsl(var(--error-text) / <alpha-value>)'},
            green: {50:'hsl(var(--success-surface) / <alpha-value>)',100:'hsl(var(--success-surface) / <alpha-value>)',200:'hsl(var(--success-border) / <alpha-value>)',700:'hsl(var(--success-text) / <alpha-value>)',800:'hsl(var(--success-text) / <alpha-value>)'},
  			foreground: 'hsl(var(--foreground))',
  			border: 'hsl(var(--border))',
  			input: 'hsl(var(--input))',
  			ring: 'hsl(var(--ring))',
  			primary: {
  				DEFAULT: 'hsl(var(--primary))',
  				foreground: 'hsl(var(--primary-foreground))'
  			},
  			secondary: {
  				DEFAULT: 'hsl(var(--secondary))',
  				foreground: 'hsl(var(--secondary-foreground))'
  			},
  			tertiary: '#64748b',
  			card: {
  				DEFAULT: 'hsl(var(--card))',
  				foreground: 'hsl(var(--card-foreground))'
  			},
  			popover: {
  				DEFAULT: 'hsl(var(--popover))',
  				foreground: 'hsl(var(--popover-foreground))'
  			},
  			muted: {
  				DEFAULT: 'hsl(var(--muted))',
  				foreground: 'hsl(var(--muted-foreground))'
  			},
  			accent: {
  				DEFAULT: 'hsl(var(--accent))',
  				foreground: 'hsl(var(--accent-foreground))'
  			},
  			destructive: {
  				DEFAULT: 'hsl(var(--destructive))',
  				foreground: 'hsl(var(--destructive-foreground))'
  			},
  			chart: {
  				'1': 'hsl(var(--chart-1))',
  				'2': 'hsl(var(--chart-2))',
  				'3': 'hsl(var(--chart-3))',
  				'4': 'hsl(var(--chart-4))',
  				'5': 'hsl(var(--chart-5))'
  			}
  		},
  		borderRadius: {
  			lg: 'var(--radius)',
  			md: 'calc(var(--radius) - 2px)',
  			sm: 'calc(var(--radius) - 4px)'
  		},
  		keyframes: {
  			'accordion-down': {
  				from: {
  					height: '0'
  				},
  				to: {
  					height: 'var(--radix-accordion-content-height)'
  				}
  			},
  			'accordion-up': {
  				from: {
  					height: 'var(--radix-accordion-content-height)'
  				},
  				to: {
  					height: '0'
  				}
  			}
  		},
  		animation: {
  			'accordion-down': 'accordion-down 0.2s ease-out',
  			'accordion-up': 'accordion-up 0.2s ease-out'
  		}
  	}
  },
  plugins: [
    require("tailwindcss-animate"),
    require("@tailwindcss/typography"),
    require("@tailwindcss/container-queries"),
  ],
}
